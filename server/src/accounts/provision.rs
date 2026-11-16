// SPDX-License-Identifier: MIT
use super::*;
#[derive(Clone)]
pub(crate) struct Capability {
    pub hash: Vec<u8>,
    pub kind: String,
    pub issuer: Option<String>,
    pub issuer_generation: Option<i64>,
    pub target: Option<String>,
    pub target_generation: Option<i64>,
    pub expiry: Option<i64>,
    pub state: String,
    pub reserved: Option<String>,
}
pub(crate) fn read_cap(c: &Connection, hash: &[u8]) -> Result<Capability> {
    c.query_row("SELECT hash,kind,issuer,issuer_generation,target,target_generation,expires_at,state,reserved_id FROM capabilities WHERE hash=?1",[hash],|r| {
        Ok(Capability {hash:r.get(0)?,kind:r.get(1)?,issuer:r.get(2)?,issuer_generation:r.get(3)?,target:r.get(4)?,target_generation:r.get(5)?,expiry:r.get(6)?,state:r.get(7)?,reserved:r.get(8)?})
    }).optional()?.filter(|cap|crypto::same_digest(&cap.hash,hash)).ok_or(Error::new("capability_invalid"))
}
pub(crate) fn valid_cap(c: &Connection, cap: &Capability, kind: &str) -> Result<()> {
    if cap.kind != kind
        || !matches!(cap.state.as_str(), "available" | "provisioning")
        || cap.expiry.is_some_and(|e| now() >= e)
    {
        return Err(Error::new("capability_invalid"));
    }
    if let Some(id) = &cap.issuer {
        read_account(c, id)?
            .filter(|a| {
                a.usable()
                    && a.identity.role == "admin"
                    && Some(a.generation) == cap.issuer_generation
            })
            .ok_or(Error::new("capability_invalid"))?;
    }
    if kind == "setup" {
        let open: bool = c.query_row(
            "SELECT phase='open' AND capability=?1 FROM setup WHERE singleton=1",
            [cap.hash.as_slice()],
            |r| r.get(0),
        )?;
        if !open {
            return Err(Error::new("capability_invalid"));
        }
    }
    if kind == "reset" {
        let id = cap
            .target
            .as_ref()
            .ok_or(Error::new("capability_invalid"))?;
        read_account(c, id)?
            .filter(|a| a.state == "ready" && Some(a.generation) == cap.target_generation)
            .ok_or(Error::new("capability_invalid"))?;
    }
    Ok(())
}
impl Service {
    pub async fn claim(
        &self,
        ip: IpAddr,
        kind: &'static str,
        token: String,
        name: &str,
        password: String,
    ) -> Result<Identity> {
        if !matches!(kind, "setup" | "invite") {
            return Err(Error::new("capability_invalid"));
        }
        self.attempts.capability(ip, &token)?;
        let hash = crypto::bearer(&token)?;
        // Clearly invalid authority fails before any password work.
        let cap = self
            .metadata
            .run(move |c| {
                admission(c)?;
                let cap = read_cap(c, &hash)?;
                valid_cap(c, &cap, kind)?;
                Ok(cap)
            })
            .await?;
        let name = crypto::username(name).map_err(|e| {
            if cap.reserved.is_some() {
                Error::new("claim_in_progress")
            } else {
                e
            }
        })?;
        if cap.reserved.is_none() {
            crypto::new_password(&password)?;
        }
        let snapshot = self
            .metadata
            .run({
                let name = name.clone();
                let cap = cap.clone();
                move |c| {
                    if let Some(id) = cap.reserved {
                        let a = read_account(c, &id)?.ok_or(Error::new("storage_unavailable"))?;
                        if a.identity.username != name {
                            return Err(Error::new("claim_in_progress"));
                        }
                        Ok(Some(a))
                    } else {
                        if c.query_row(
                            "SELECT EXISTS(SELECT 1 FROM accounts WHERE username=?1)",
                            [name],
                            |r| r.get::<_, bool>(0),
                        )? {
                            return Err(Error::new("username_unavailable"));
                        }
                        Ok(None)
                    }
                }
            })
            .await?;
        let phc = if let Some(a) = snapshot.as_ref() {
            let supplied = password.clone();
            let reserved = a.phc.clone();
            if !self
                .hashes
                .run(move || Ok(crypto::verify(&supplied, &reserved)))
                .await?
            {
                return Err(Error::new("claim_in_progress"));
            }
            a.phc.clone()
        } else {
            let supplied = password.clone();
            self.hashes.run(move || crypto::hash(&supplied)).await?
        };
        #[cfg(feature = "test-hooks")]
        self.pause("claim_before_reservation").await;
        let id = crypto::random_token()?;
        let account=self.metadata.run({let name=name.clone(); move |c| {
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?; admission(&tx)?;
            let cap=read_cap(&tx,&hash)?; valid_cap(&tx,&cap,kind)?;
            let account=if let Some(id)=cap.reserved {
                let a=read_account(&tx,&id)?.ok_or(Error::new("storage_unavailable"))?;
                if a.identity.username!=name { return Err(Error::new("claim_in_progress")); } a
            } else {
                if tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE username=?1)",[&name],|r|r.get::<_,bool>(0))? { return Err(Error::new("username_unavailable")); }
                let role=if kind=="setup" {"admin"} else {"member"};
                tx.execute("INSERT INTO accounts VALUES(?1,?2,?3,'provisioning',1,?4,1,1)",params![id,name,role,phc])?;
                tx.execute("UPDATE capabilities SET state='provisioning',reserved_id=?2 WHERE hash=?1",params![hash.as_slice(),id])?;
                tx.execute("INSERT INTO provisioning_reservations VALUES(?1,?2,?3,?4,?5)",params![hash.as_slice(),id,name,role,crypto::digest(phc.as_bytes()).as_slice()])?;
                read_account(&tx,&id)?.ok_or(Error::new("storage_unavailable"))?
            };
            tx.commit()?; phase("after_reserve")?; Ok(account)
        }}).await?;
        self.pause("claim_after_reserve").await;
        // A concurrent claim may have reserved a different password after our
        // first read. Verify that exact stored PHC, never overwrite its proof.
        let phc = account.phc.clone();
        if !self
            .hashes
            .run(move || Ok(crypto::verify(&password, &phc)))
            .await?
        {
            return Err(Error::new("claim_in_progress"));
        }
        self.finish_provision(hash, kind, account).await
    }
    async fn finish_provision(
        &self,
        hash: [u8; 32],
        kind: &'static str,
        account: Account,
    ) -> Result<Identity> {
        let gate = self.barriers.get(&account.identity.id)?;
        let guard = gate.enter().await;
        let budget = self.temp.clone();
        let staged_id = account.identity.id.clone();
        let (gate, guard) = self
            .metadata
            .files(move |root| {
                // Return ownership with the completed blocking job. Cancellation
                // cannot release the barrier while its writes are still running.
                let db = crate::storage::open(root.database(), false)?;
                admission(&db)?;
                let cap = read_cap(&db, &hash)?;
                valid_cap(&db, &cap, kind)?;
                if cap.state != "provisioning"
                    || cap.reserved.as_deref() != Some(staged_id.as_str())
                {
                    return Err(Error::new("capability_invalid"));
                }
                root.provision(&staged_id, &budget)
                    .map_err(|_| Error::new("provision_unavailable"))?;
                phase("after_stores")?;
                Ok((gate, guard))
            })
            .await?;
        #[cfg(feature = "test-hooks")]
        self.pause("claim_after_stores").await;
        self.metadata
            .files(move |root| {
                let _publication = (gate, guard);
                let mut db = crate::storage::open(root.database(), false)?;
                root.validate_stores(&account.identity.id)
                    .map_err(|_| Error::new("provision_unavailable"))?;
                phase("before_ready")?;
                root.validate_stores(&account.identity.id)
                    .map_err(|_| Error::new("provision_unavailable"))?;
                let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                let cap = read_cap(&tx, &hash)?;
                valid_cap(&tx, &cap, kind)?;
                if cap.state != "provisioning"
                    || cap.reserved.as_deref() != Some(account.identity.id.as_str())
                {
                    return Err(Error::new("capability_invalid"));
                }
                let a = read_account(&tx, &account.identity.id)?
                    .filter(|a| {
                        a.state == "provisioning"
                            && a.revision == account.revision
                            && a.generation == account.generation
                    })
                    .ok_or(Error::new("capability_invalid"))?;
                tx.execute(
                    "UPDATE accounts SET provisioning_state='ready' WHERE id=?1",
                    [&a.identity.id],
                )?;
                tx.execute(
                    "UPDATE capabilities SET state='consumed' WHERE hash=?1",
                    [hash.as_slice()],
                )?;
                if kind == "setup" {
                    tx.execute(
                        "UPDATE setup SET phase='closed' WHERE singleton=1 AND phase='open'",
                        [],
                    )?;
                }
                root.validate()?;
                tx.commit()?;
                phase("after_ready")?;
                // Closure is durable before unlink. Failure leaves a powerless file.
                if kind == "setup" {
                    let _ = root.remove_setup_file();
                }
                Ok(a.identity)
            })
            .await
    }
    pub(crate) async fn resume_startup(&self) -> Result<()> {
        let reserved=self.metadata.run(|c| {
            let mut q=c.prepare("SELECT hash FROM capabilities WHERE reserved_id IS NOT NULL AND state IN ('provisioning','revoked') ORDER BY reserved_id")?;
            let rows=q.query_map([],|r|r.get::<_,Vec<u8>>(0))?;
            let mut result=Vec::new();
            for h in rows {
                let h=h?; let cap=read_cap(c,&h)?;
                // Closed restore admission never resumes provisioning.
                let a=read_account(c,cap.reserved.as_deref().ok_or(Error::new("storage_unavailable"))?)?.ok_or(Error::new("storage_unavailable"))?;
                match admission(c).and_then(|_|valid_cap(c,&cap,&cap.kind)) {
                    Ok(())=>{},
                    Err(e) if matches!(e.code,"capability_invalid"|"admission_closed")=>{
                        eprintln!("provision account={} phase=blocked",a.identity.id); continue;
                    },
                    Err(e)=>return Err(e),
                }
                result.push((h,cap.kind,a));
            }
            Ok(result)
        }).await?;
        for (hash, kind, a) in reserved {
            let hash: [u8; 32] = hash
                .try_into()
                .map_err(|_| Error::new("storage_unavailable"))?;
            let kind = match kind.as_str() {
                "setup" => "setup",
                "invite" => "invite",
                _ => return Err(Error::new("storage_unavailable")),
            };
            self.finish_provision(hash, kind, a).await?;
        }
        // Validate every ready store, not a guessed empty replacement.
        let ids = self
            .metadata
            .run(|c| {
                let mut q =
                    c.prepare("SELECT id FROM accounts WHERE provisioning_state='ready'")?;
                Ok(q.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await?;
        self.metadata
            .files(move |root| {
                for id in ids {
                    root.validate_stores(&id)?;
                }
                Ok(())
            })
            .await
    }
}
