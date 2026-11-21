// SPDX-License-Identifier: MIT
use super::*;

const BROWSER_SESSION_SECONDS: i64 = 365 * 86400;
const BROWSER_RENEW_INTERVAL_SECONDS: i64 = 86400;

impl Service {
    pub async fn browser_authority(&self, token: &str) -> Result<BrowserAuthority> {
        let hash = crypto::bearer(token).map_err(|_| Error::new("authentication_failed"))?;
        self.metadata
            .run(move |c| {
                admission(c)?;
                let id: Option<String> = c
                    .query_row(
                        "SELECT account_id FROM credentials WHERE hash=?1 AND kind='session'",
                        [hash.as_slice()],
                        |r| r.get(0),
                    )
                    .optional()?;
                let account = read_account(c, &id.ok_or(Error::new("authentication_failed"))?)?
                    .ok_or(Error::new("authentication_failed"))?;
                let a = BrowserAuthority { account, hash };
                check_browser(c, &a, "authentication_failed")?;
                Ok(a)
            })
            .await
    }
    pub async fn validate_hostkey(&self, hostkey: &str) -> Result<NativeAuthority> {
        let hash = crypto::bearer(hostkey).map_err(|_| Error::new("authentication_failed"))?;
        self.metadata.run(move |c| {
            admission(c)?;
            let row:Option<(String,i64,Vec<u8>)>=c.query_row("SELECT account_id,generation,hash FROM credentials WHERE hash=?1 AND kind='hostkey' AND revoked=0",[hash.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let (id,generation,stored)=row.ok_or(Error::new("authentication_failed"))?;
            if !crypto::same_digest(&stored,&hash) { return Err(Error::new("authentication_failed")); }
            let data_epoch=crate::storage::reset::admitted_epoch(c,&id)?;
            let a=NativeAuthority{id,generation,data_epoch}; barrier::check_native_generation(c,&a)?; Ok(a)
        }).await
    }
    pub async fn browser_login(
        &self,
        ip: IpAddr,
        name: &str,
        password: String,
    ) -> Result<(Identity, String)> {
        let (identity, token, _) = self.browser_login_session(ip, name, password).await?;
        Ok((identity, token))
    }
    pub(crate) async fn browser_login_session(
        &self,
        ip: IpAddr,
        name: &str,
        password: String,
    ) -> Result<(Identity, String, i64)> {
        let (identity, token, expiry) = self.login(ip, name, password, "session").await?;
        Ok((identity, token, expiry.expect("browser session expiry")))
    }
    /// Separate from browser cookies; U5 maps generic auth failure to native 403.
    pub async fn native_login(
        &self,
        ip: IpAddr,
        name: &str,
        password: String,
    ) -> Result<(Identity, String)> {
        let (identity, token, _) = self.login(ip, name, password, "hostkey").await?;
        Ok((identity, token))
    }
    async fn login(
        &self,
        ip: IpAddr,
        name: &str,
        password: String,
        kind: &'static str,
    ) -> Result<(Identity, String, Option<i64>)> {
        let name = self.name_attempt(ip, name).map_err(|e| {
            if e.code == "validation_failed" {
                Error::new("authentication_failed")
            } else {
                e
            }
        })?;
        let original = self
            .metadata
            .run(move |c| {
                admission(c)?;
                let id: Option<String> = c
                    .query_row("SELECT id FROM accounts WHERE username=?1", [name], |r| {
                        r.get(0)
                    })
                    .optional()?;
                match id {
                    Some(id) => Ok(read_account(c, &id)?.filter(Account::usable)),
                    None => Ok(None),
                }
            })
            .await?;
        // Public synthetic PHC with the production cost. Unknown/disabled names
        // enter exactly the same bounded pool; even a matching dummy proof never
        // supplies an identity. No per-request dummy hash generation is needed.
        const DUMMY_PHC: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAECAwQFBgcICQoLDA0ODw$krbozlfmG/YbBoj1y9KHSlgK7HHNhVNfzFmZ8bEcWH4";
        let phc = original
            .as_ref()
            .map_or(DUMMY_PHC, |a| a.phc.as_str())
            .to_owned();
        if !self
            .hashes
            .run(move || Ok(crypto::verify(&password, &phc)))
            .await?
        {
            return Err(Error::new("authentication_failed"));
        }
        let original = original.ok_or(Error::new("authentication_failed"))?;
        self.pause("login_after_proof").await;
        let gate = self.barriers.get(&original.identity.id)?;
        let guard = gate.enter().await;
        #[cfg(feature = "test-hooks")]
        self.pause("login_with_barrier").await;
        let token = crypto::random_token()?;
        let hash = crypto::bearer(&token)?;
        let (identity,expiry)=self.metadata.run(move |c| {
            let _publication=(gate,guard);
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?; admission(&tx)?;
            let current=read_account(&tx,&original.identity.id)?.filter(|a|a.usable() && a.revision==original.revision && a.generation==original.generation && a.phc==original.phc).ok_or(Error::new("authentication_failed"))?;
            let expiry=if kind=="session" {Some(now()+BROWSER_SESSION_SECONDS)} else {None};
            tx.execute("INSERT INTO credentials(hash,kind,account_id,generation,expires_at) VALUES(?1,?2,?3,?4,?5)",params![hash.as_slice(),kind,current.identity.id,current.generation,expiry])?;
            tx.commit()?; Ok((current.identity,expiry))
        }).await?;
        Ok((identity, token, expiry))
    }
    /// Only the browser identity endpoint rolls sessions. Ordinary identity and
    /// authority reads stay read-only, including before the daily boundary.
    pub(crate) async fn browser_me(&self, authority: BrowserAuthority) -> Result<(Identity, i64)> {
        #[cfg(feature = "test-hooks")]
        let hooks = self.hooks.clone();
        self.metadata.run(move |c| {
            admission(c)?;
            let checked_at = now();
            let (account, expires_at) =
                check_browser_expiry(c, &authority, "authentication_failed", checked_at)?;
            let renewal_threshold = BROWSER_SESSION_SECONDS - BROWSER_RENEW_INTERVAL_SECONDS;
            if expires_at - checked_at > renewal_threshold {
                return Ok((account.identity, expires_at));
            }

            // A competing renewal or revocation may have won while acquiring
            // the writer. Recheck original authority and expiry at a fresh time.
            #[cfg(feature = "test-hooks")]
            tokio::runtime::Handle::current()
                .block_on(hooks.hit("browser_session_before_renewal"));
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            admission(&tx)?;
            let checked_at = now();
            let (account, mut expires_at) =
                check_browser_expiry(&tx, &authority, "authentication_failed", checked_at)?;
            if expires_at - checked_at <= renewal_threshold {
                expires_at = checked_at + BROWSER_SESSION_SECONDS;
                tx.execute(
                    "UPDATE credentials SET expires_at=?2 WHERE hash=?1 AND kind='session'",
                    params![authority.hash.as_slice(), expires_at],
                )?;
            }
            tx.commit()?;
            Ok((account.identity, expires_at))
        }).await
    }
    pub async fn logout(&self, token: &str) -> Result<()> {
        let hash = crypto::bearer(token).map_err(|_| Error::new("authentication_failed"))?;
        self.metadata
            .run(move |c| {
                admission(c)?;
                // Keep revoked digest records, so a formerly valid cookie remains
                // idempotently clearable without granting any renewed authority.
                let exists = c
                    .query_row(
                        "SELECT hash FROM credentials WHERE hash=?1 AND kind='session'",
                        [hash.as_slice()],
                        |r| r.get::<_, Vec<u8>>(0),
                    )
                    .optional()?;
                if !exists.is_some_and(|v| crypto::same_digest(&v, &hash)) {
                    return Err(Error::new("authentication_failed"));
                }
                c.execute(
                    "UPDATE credentials SET revoked=1 WHERE hash=?1 AND kind='session'",
                    [hash.as_slice()],
                )?;
                Ok(())
            })
            .await
    }
    pub async fn change_password(
        &self,
        ip: IpAddr,
        authority: BrowserAuthority,
        current: String,
        new: String,
    ) -> Result<()> {
        self.attempts
            .username(ip, &authority.account.identity.username)?;
        crypto::new_password(&new).map_err(|_| Error::field("new_password"))?;
        let original = self
            .metadata
            .run({
                let a = authority.clone();
                move |c| {
                    admission(c)?;
                    check_browser(c, &a, "authentication_failed")
                }
            })
            .await?;
        let phc = original.phc.clone();
        let replacement = self
            .hashes
            .run(move || {
                if !crypto::verify(&current, &phc) {
                    return Err(Error::new("authentication_failed"));
                }
                crypto::hash(&new)
            })
            .await?;
        self.pause("password_after_proof").await;
        let gate = self.barriers.get(&original.identity.id)?;
        let guard = gate.enter().await;
        self.pause("password_with_barrier").await;
        self.metadata
            .run(move |c| {
                let _guard = guard;
                let _gate = gate.clone();
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                let current = check_browser(&tx, &authority, "authentication_failed")?;
                if current.revision != original.revision
                    || current.generation != original.generation
                    || current.phc != original.phc
                {
                    return Err(Error::new("authentication_failed"));
                }
                gate.retire()?;
                replace_password(&tx, &current.identity.id, &replacement)?;
                tx.commit()?;
                Ok(())
            })
            .await
    }
    /// Stopped-service recovery uses the same owner lock as serve. A Service is
    /// not needed or instantiated, so no HTTP listener can coexist with this.
    pub async fn recover_admin(
        root: DataRoot,
        id: String,
        password: String,
        reenable: bool,
    ) -> Result<()> {
        crypto::new_password(&password)?;
        let metadata = Metadata::new(Arc::new(root));
        metadata.files(|r| {
            r.upgrade_media()?;
            r.upgrade_deletions()?;
            r.upgrade_oauth()?;
            r.upgrade_resets()
        }).await?;
        let phc = HashPool::default()
            .run(move || crypto::hash(&password))
            .await?;
        metadata
            .run(move |c| {
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let a = read_account(&tx, &id)?
                    .filter(|a| a.identity.role == "admin" && a.state == "ready")
                    .ok_or(Error::new("permission_denied"))?;
                replace_password(&tx, &a.identity.id, &phc)?;
                if reenable {
                    tx.execute("UPDATE accounts SET enabled=1 WHERE id=?1", [id])?;
                }
                // Deliberately leave service_state.admission_open untouched.
                tx.commit()?;
                Ok(())
            })
            .await
    }
}
