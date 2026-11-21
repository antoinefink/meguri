// SPDX-License-Identifier: MIT
use super::*;
use crate::storage::deletion::{self, Pending};

/// Resolve before allocating a gate. Pending IDs authorize only resuming their
/// persisted cleanup; a guessed or already completed ID never creates a gate.
fn resolve(c: &Connection, id: &str) -> Result<Option<Account>> {
    if crate::storage::reset::read(c, id)?.is_some() { return Err(Error::new("storage_unavailable")); }
    if let Some(account) = read_account(c, id)? {
        if account.identity.role != "member" { return Err(Error::new("permission_denied")); }
        if deletion::read(c, id)?.is_some() { return Err(Error::new("storage_unavailable")); }
        Ok(Some(account))
    } else {
        deletion::read(c, id)?.ok_or(Error::new("route_not_found"))?;
        Ok(None)
    }
}
impl Service {
    pub async fn delete_member(&self, authority: BrowserAuthority, id: String) -> Result<()> {
        self.metadata.run({
            let id = id.clone();
            let actor = authority.clone();
            move |c| {
                admission(c)?;
                check_admin(c, &actor, "stale_authority")?;
                resolve(c, &id)?;
                Ok(())
            }
        }).await?;
        self.pause("delete_before_commit").await;
        let gate = self.barriers.get(&id)?;
        let guard = gate.enter().await;
        self.pause("delete_with_barrier").await;
        self.metadata.files(move |root| {
            // This entire blocking operation owns both objects even when the
            // request future is cancelled. No client waits span this scope.
            let _publication = (gate.clone(), guard);
            let job = {
                let mut c = crate::storage::open(root.database(), false)?;
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                check_admin(&tx, &authority, "stale_authority")?;
                let job = if let Some(target) = resolve(&tx, &id)? {
                    // Unexpected issuer/setup references are never broadened
                    // into a cascade: preserve them and fail closed.
                    if tx.query_row("SELECT EXISTS(SELECT 1 FROM capabilities WHERE issuer=?1 OR (kind='setup' AND reserved_id=?1))", [&id], |r| r.get::<_,bool>(0))? {
                        return Err(Error::new("storage_unavailable"));
                    }
                    let binding = root.account_deletion_binding(&id, target.state == "provisioning")?;
                    gate.retire()?;
                    let job = Pending { id: id.clone(), binding };
                    deletion::insert(&tx, &job)?;
                    tx.execute("DELETE FROM media_names WHERE account_id=?1", [&id])?;
                    tx.execute("DELETE FROM media_blobs WHERE account_id=?1", [&id])?;
                    tx.execute("DELETE FROM credentials WHERE account_id=?1", [&id])?;
                    tx.execute("DELETE FROM provisioning_reservations WHERE account_id=?1", [&id])?;
                    tx.execute("DELETE FROM capabilities WHERE kind='invite' AND reserved_id=?1", [&id])?;
                    tx.execute("DELETE FROM capabilities WHERE kind='reset' AND target=?1", [&id])?;
                    if tx.execute("DELETE FROM accounts WHERE id=?1 AND role='member'", [&id])? != 1 {
                        return Err(Error::new("storage_unavailable"));
                    }
                    job
                } else {
                    deletion::read(&tx, &id)?.ok_or(Error::new("route_not_found"))?
                };
                phase("delete_before_metadata_commit")?;
                tx.commit()?;
                job
            };
            phase("delete_after_metadata_commit")?;
            deletion::finish(root, &job)
        }).await
    }
    pub(crate) async fn recover_deletions(&self) -> Result<()> {
        let jobs = self.metadata.run(|c| {
            // Validate every intent even if restore admission is closed.
            let jobs = deletion::pending(c)?;
            match admission(c) {
                Ok(()) => Ok(jobs),
                Err(e) if e.code == "admission_closed" => Ok(Vec::new()),
                Err(e) => Err(e),
            }
        }).await?;
        for job in jobs {
            let gate = self.barriers.get(&job.id)?;
            let guard = gate.enter().await;
            self.metadata.files(move |root| {
                let _publication = (gate, guard);
                // Restore closure is authoritative even on restart recovery.
                let c = crate::storage::open(root.database(), false)?;
                admission(&c)?;
                drop(c);
                deletion::finish(root, &job)
            }).await?;
        }
        Ok(())
    }
}
