// SPDX-License-Identifier: MIT
//! Reset keeps identity and credentials. Only a durable purge intent permits
//! removal; after the recreate transition no retry can invoke removal again.
use super::{DataRoot, root::unavailable};
use crate::{Result, crypto};
use rusqlite::{Connection, OptionalExtension};

const MIGRATION: &str = "
CREATE TABLE account_data_epochs (
 account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 epoch INTEGER NOT NULL CHECK(typeof(epoch)='integer' AND epoch>0)
);
CREATE TABLE account_resets (
 id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
 device TEXT NOT NULL,
 inode TEXT NOT NULL,
 epoch INTEGER NOT NULL CHECK(typeof(epoch)='integer' AND epoch>0),
 phase TEXT NOT NULL CHECK(phase IN ('purge','recreate'))
);
PRAGMA user_version=5;
";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    pub id: String,
    pub binding: (u64,u64),
    pub epoch: i64,
    pub phase: String,
}
fn supported(c: &Connection) -> Result<bool> {
    Ok(c.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0))? >= 5)
}
pub(crate) fn epoch(c: &Connection, id: &str) -> Result<i64> {
    if !supported(c)? { return Ok(0); }
    let epoch = c.query_row("SELECT epoch FROM account_data_epochs WHERE account_id=?1",[id],|r|r.get::<_,i64>(0)).optional()?;
    if epoch.is_some_and(|e|e<=0) { return Err(unavailable()); }
    Ok(epoch.unwrap_or(0))
}
pub(crate) fn admitted_epoch(c: &Connection, id: &str) -> Result<i64> {
    if read(c,id)?.is_some() { return Err(unavailable()); }
    epoch(c,id)
}
fn decode(row: &rusqlite::Row<'_>) -> Result<Pending> {
    let id: String = row.get(0)?;
    crypto::bearer(&id).map_err(|_|unavailable())?;
    let number = |value: String| -> Result<u64> {
        let parsed = value.parse::<u64>().map_err(|_|unavailable())?;
        if parsed.to_string()!=value { return Err(unavailable()); }
        Ok(parsed)
    };
    let job = Pending { id, binding:(number(row.get(1)?)?,number(row.get(2)?)?), epoch:row.get(3)?,phase:row.get(4)? };
    if job.epoch<=0 || !matches!(job.phase.as_str(),"purge"|"recreate") { return Err(unavailable()); }
    Ok(job)
}
pub(crate) fn read(c: &Connection, id: &str) -> Result<Option<Pending>> {
    if !supported(c)? { return Ok(None); }
    let mut q=c.prepare("SELECT id,device,inode,epoch,phase FROM account_resets WHERE id=?1")?;
    let mut rows=q.query([id])?;
    rows.next()?.map(decode).transpose()
}
/// Complete-content validation precedes any recovery or ready-store exception.
pub(crate) fn pending(c: &Connection) -> Result<Vec<Pending>> {
    if !supported(c)? { return Ok(Vec::new()); }
    if c.prepare("SELECT 1 FROM account_data_epochs e LEFT JOIN accounts a ON a.id=e.account_id WHERE a.id IS NULL OR typeof(e.epoch)<>'integer' OR e.epoch<=0")?.exists([])? { return Err(unavailable()); }
    let mut q=c.prepare("SELECT id,device,inode,epoch,phase FROM account_resets ORDER BY id")?;
    let mut rows=q.query([])?;
    let mut jobs=Vec::new();
    while let Some(row)=rows.next()? {
        let job=decode(row)?;
        let valid: bool=c.query_row("SELECT EXISTS(SELECT 1 FROM accounts a JOIN account_data_epochs e ON e.account_id=a.id WHERE a.id=?1 AND a.provisioning_state='ready' AND a.media_usn=0 AND e.epoch=?2)
            AND NOT EXISTS(SELECT 1 FROM account_deletions WHERE id=?1)
            AND NOT EXISTS(SELECT 1 FROM media_names WHERE account_id=?1)
            AND NOT EXISTS(SELECT 1 FROM media_blobs WHERE account_id=?1)",rusqlite::params![job.id,job.epoch],|r|r.get(0))?;
        if !valid { return Err(unavailable()); }
        jobs.push(job);
    }
    Ok(jobs)
}
pub(crate) fn insert(c: &Connection, job: &Pending) -> Result<()> {
    c.execute("INSERT INTO account_resets VALUES(?1,?2,?3,?4,'purge')",rusqlite::params![job.id,job.binding.0.to_string(),job.binding.1.to_string(),job.epoch])?;
    Ok(())
}
/// Caller owns the target barrier, including across cancellation. Recursive
/// removal and provisioning hold no metadata writer or global directory mutex.
pub(crate) fn finish(root: &DataRoot, mut job: Pending, reservation: super::root::ProvisionReservation) -> Result<Option<crate::accounts::AccountStorage>> {
    if job.phase=="purge" {
        root.remove_account_directory(&job.id,Some(job.binding))?;
        crate::accounts::phase("reset_data_after_purge")?;
        let mut c=super::open(root.database(),false)?;
        let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::accounts::admission(&tx)?;
        if read(&tx,&job.id)?.as_ref()!=Some(&job) { return Err(unavailable()); }
        tx.execute("UPDATE account_resets SET phase='recreate' WHERE id=?1",[&job.id])?;
        tx.commit()?;
        job.phase="recreate".into();
    }
    crate::accounts::phase("reset_data_before_provision")?;
    root.provision_reserved(reservation)?;
    crate::accounts::phase("reset_data_after_provision")?;
    let storage=super::collection::storage(root,&job.id,0).ok();
    let mut c=super::open(root.database(),false)?;
    let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    crate::accounts::admission(&tx)?;
    if read(&tx,&job.id)?.as_ref()!=Some(&job) { return Err(unavailable()); }
    tx.execute("DELETE FROM account_resets WHERE id=?1",[&job.id])?;
    crate::accounts::phase("reset_data_before_journal_clear")?;
    tx.commit()?;
    Ok(storage)
}
impl DataRoot {
    pub(crate) fn upgrade_resets(&self) -> Result<()> {
        self.check_initialized()?;
        let mut c=super::open(self.database(),false)?;
        let version:i64=c.pragma_query_value(None,"user_version",|r|r.get(0))?;
        if version==5 { return Ok(()); }
        if version!=4 { return Err(unavailable()); }
        let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(MIGRATION)?;
        crate::accounts::phase("reset_data_migration_before_commit")?;
        tx.commit()?;
        self.check_initialized()
    }
}

#[cfg(all(test,feature="test-hooks"))]
mod tests {
    use super::*;
    #[test]
    fn reset_migration_failure_keeps_v4_and_is_reentrant_without_relaxing_validation() {
        let temp=tempfile::tempdir().unwrap();
        let mut root=DataRoot::acquire(&temp.path().join("owner"),true).unwrap();
        root.init().unwrap(); root.upgrade_media().unwrap(); root.upgrade_deletions().unwrap(); root.upgrade_oauth().unwrap();
        assert!(crate::test_hooks::storage_failure("reset_data_migration_before_commit",||root.upgrade_resets()).is_err());
        let c=super::super::open(root.database(),false).unwrap();
        assert_eq!(c.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),4);
        assert!(!c.prepare("SELECT 1 FROM sqlite_schema WHERE name IN ('account_resets','account_data_epochs')").unwrap().exists([]).unwrap());
        drop(c); root.check_initialized().unwrap();
        root.upgrade_resets().unwrap(); root.upgrade_resets().unwrap(); root.check_initialized().unwrap();
        let c=super::super::open(root.database(),false).unwrap();
        assert_eq!(c.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),5);
        assert!(pending(&c).unwrap().is_empty());
    }
    #[test]
    fn intent_decoder_rejects_noncanonical_identity_binding_and_phase() {
        let c=Connection::open_in_memory().unwrap();
        let id=crypto::random_token().unwrap();
        for (device,inode,epoch,phase) in [("01","2",1,"purge"),("-1","2",1,"purge"),("1","18446744073709551616",1,"purge"),("1","2",0,"purge"),("1","2",1,"unknown")] {
            let mut q=c.prepare("SELECT ?1,?2,?3,?4,?5").unwrap();
            let mut rows=q.query(rusqlite::params![id,device,inode,epoch,phase]).unwrap();
            assert!(decode(rows.next().unwrap().unwrap()).is_err());
        }
    }
}
