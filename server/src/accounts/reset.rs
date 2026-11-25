// SPDX-License-Identifier: MIT
use super::*;
use crate::storage::reset::{self, Pending};

fn target(c: &Connection, id: &str) -> Result<Account> {
    if crate::storage::backups::pending(c,id)? {return Err(Error::new("storage_unavailable"));}
    read_account(c,id)?.filter(|a|a.state=="ready").ok_or(Error::new("route_not_found"))
}
impl Service {
    pub async fn reset_data(&self, authority: BrowserAuthority, id: String) -> Result<AccountSummary> {
        let (expected, resuming)=self.metadata.run({
            let id=id.clone(); let actor=authority.clone();
            move |c| {
                let tx=c.transaction()?;
                admission(&tx)?;
                check_admin(&tx,&actor,"stale_authority")?;
                target(&tx,&id)?;
                Ok((reset::epoch(&tx,&id)?, reset::read(&tx,&id)?))
            }
        }).await?;
        self.pause("reset_data_before_commit").await;
        let gate=self.barriers.get(&id)?;
        let guard=gate.enter().await;
        self.pause("reset_data_with_barrier").await;
        let budget=self.temp.clone();
        self.metadata.files(move |root| {
            let _publication=(gate.clone(),guard);
            let (job,account)={
                let mut c=crate::storage::open(root.database(),false)?;
                let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                // Never substitute a newer administrator/session after waiting.
                check_admin(&tx,&authority,"stale_authority")?;
                let account=target(&tx,&id)?;
                if reset::epoch(&tx,&id)?!=expected { return Err(Error::new("storage_unavailable")); }
                let pending=reset::read(&tx,&id)?;
                let job=if let Some(original)=resuming {
                    // A queued retry of an admitted reset must never become a
                    // second purge after the first worker has recreated data.
                    if pending.as_ref().is_some_and(|job| job.epoch!=original.epoch || job.binding!=original.binding) {
                        return Err(Error::new("storage_unavailable"));
                    }
                    match pending {
                        Some(job)=>Some((job,root.reserve_provision(&id,&budget)?)),
                        None=>None,
                    }
                } else if pending.is_some() {
                    return Err(Error::new("storage_unavailable"));
                } else {
                    // Refuse known staging exhaustion before retiring authority
                    // or changing the epoch, media index, journal or files.
                    let reservation=root.reserve_provision(&id,&budget)?;
                    let binding=root.account_deletion_binding(&id,false)?.ok_or(Error::new("storage_unavailable"))?;
                    let epoch=expected.checked_add(1).ok_or(Error::new("storage_unavailable"))?;
                    gate.retire()?;
                    crate::storage::backups::clear(&tx,&id)?;
                    tx.execute("DELETE FROM media_names WHERE account_id=?1",[&id])?;
                    tx.execute("DELETE FROM media_blobs WHERE account_id=?1",[&id])?;
                    tx.execute("UPDATE accounts SET media_usn=0 WHERE id=?1",[&id])?;
                    tx.execute("INSERT INTO account_data_epochs VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET epoch=excluded.epoch",params![id,epoch])?;
                    let job=Pending {id:id.clone(),binding,epoch,phase:"purge".into()};
                    reset::insert(&tx,&job)?;
                    Some((job,reservation))
                };
                phase("reset_data_before_metadata_commit")?;
                tx.commit()?;
                (job,account)
            };
            phase("reset_data_after_metadata_commit")?;
            let storage=match job {
                Some((job,reservation))=>reset::finish(root,job,reservation)?,
                None=> {
                    let c=crate::storage::open(root.database(),false)?;
                    let media=crate::media::index::storage_bytes(&c,&id).ok();
                    drop(c);
                    media.and_then(|bytes|crate::storage::collection::storage(root,&id,bytes).ok())
                },
            };
            Ok(AccountSummary {identity:account.identity,enabled:account.enabled,provisioning_state:account.state,storage})
        }).await
    }
    pub(crate) async fn recover_resets(&self) -> Result<()> {
        let jobs=self.metadata.run(|c| {
            let jobs=reset::pending(c)?;
            match admission(c) {
                Ok(())=>Ok(jobs),
                Err(e) if e.code=="admission_closed"=>Ok(Vec::new()),
                Err(e)=>Err(e),
            }
        }).await?;
        for job in jobs {
            let gate=self.barriers.get(&job.id)?;
            let guard=gate.enter().await;
            let budget=self.temp.clone();
            self.metadata.files(move |root| {
                let _publication=(gate,guard);
                let c=crate::storage::open(root.database(),false)?;
                admission(&c)?;
                drop(c);
                let reservation=root.reserve_provision(&job.id,&budget)?;
                reset::finish(root,job,reservation).map(|_|())
            }).await?;
        }
        Ok(())
    }
}
