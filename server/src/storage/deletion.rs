// SPDX-License-Identifier: MIT
//! Durable, account-scoped cleanup intents. Identity strings avoid narrowing
//! filesystem u64 device/inode values to SQLite's signed integer range.
use super::{DataRoot, root::unavailable};
use crate::{Result, crypto};
use rusqlite::{Connection, OptionalExtension, params};

const MIGRATION: &str = "
CREATE TABLE account_deletions (
 id TEXT PRIMARY KEY NOT NULL,
 device TEXT,
 inode TEXT,
 CHECK((device IS NULL AND inode IS NULL) OR
       (device IS NOT NULL AND inode IS NOT NULL))
);
DROP TRIGGER reservation_retained;
CREATE TRIGGER reservation_retained BEFORE DELETE ON provisioning_reservations
 WHEN NOT EXISTS(SELECT 1 FROM account_deletions WHERE id=OLD.account_id)
 BEGIN SELECT RAISE(ABORT,'retained reservation'); END;
CREATE TRIGGER deletion_id_reserved BEFORE INSERT ON accounts
 WHEN EXISTS(SELECT 1 FROM account_deletions WHERE id=NEW.id)
 BEGIN SELECT RAISE(ABORT,'pending deletion'); END;
PRAGMA user_version=3;
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pending {
    pub id: String,
    pub binding: Option<(u64, u64)>,
}
fn number(text: String) -> Result<u64> {
    let value = text.parse::<u64>().map_err(|_| unavailable())?;
    if value.to_string() != text { return Err(unavailable()); }
    Ok(value)
}
fn decode(row: &rusqlite::Row<'_>) -> Result<Pending> {
    let id: String = row.get(0)?;
    crypto::bearer(&id).map_err(|_| unavailable())?;
    let device: Option<String> = row.get(1)?;
    let inode: Option<String> = row.get(2)?;
    let binding = match (device, inode) {
        (None, None) => None,
        (Some(device), Some(inode)) => Some((number(device)?, number(inode)?)),
        _ => return Err(unavailable()),
    };
    Ok(Pending { id, binding })
}
pub(crate) fn read(c: &Connection, id: &str) -> Result<Option<Pending>> {
    let mut q = c.prepare("SELECT id,device,inode FROM account_deletions WHERE id=?1")?;
    let mut rows = q.query([id])?;
    rows.next()?.map(decode).transpose()
}
/// Read and validate the complete journal before cleaning any entry.
pub(crate) fn pending(c: &Connection) -> Result<Vec<Pending>> {
    let mut q = c.prepare("SELECT id,device,inode FROM account_deletions ORDER BY id")?;
    let mut rows = q.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let job = decode(row)?;
        if c.query_row("SELECT 1 FROM accounts WHERE id=?1", [&job.id], |_| Ok(())).optional()?.is_some() {
            return Err(unavailable());
        }
        result.push(job);
    }
    Ok(result)
}
pub(crate) fn insert(c: &Connection, job: &Pending) -> Result<()> {
    let (device, inode) = match job.binding {
        Some((device, inode)) => (Some(device.to_string()), Some(inode.to_string())),
        None => (None, None),
    };
    c.execute("INSERT INTO account_deletions VALUES(?1,?2,?3)", params![job.id, device, inode])?;
    Ok(())
}
/// No metadata writer or global directory-pin lock spans recursive cleanup.
/// Caller retains the account publication barrier for this entire operation.
pub(crate) fn finish(root: &DataRoot, job: &Pending) -> Result<()> {
    root.remove_account_directory(&job.id, job.binding)?;
    crate::accounts::phase("delete_before_journal_clear")?;
    let mut c = super::open(root.database(), false)?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    crate::accounts::admission(&tx)?;
    if read(&tx, &job.id)?.as_ref() != Some(job) { return Err(unavailable()); }
    tx.execute("DELETE FROM account_deletions WHERE id=?1", [&job.id])?;
    tx.commit()?;
    Ok(())
}
impl DataRoot {
    pub(crate) fn upgrade_deletions(&self) -> Result<()> {
        // Only called after upgrade_media has validated the complete store.
        let mut c = super::open(self.database(), false)?;
        let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if matches!(version, 3 | 4 | 5) { return Ok(()); }
        if version == 2 {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch(MIGRATION)?;
            crate::accounts::phase("delete_migration_before_commit")?;
            tx.commit()?;
        } else { return Err(unavailable()); }
        self.check_initialized()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-hooks")]
    #[test]
    fn migration_failure_retains_v2_then_v3_keeps_reservation_exception_exact() {
        let temp = tempfile::tempdir().unwrap();
        let mut root = DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
        root.init().unwrap();
        root.upgrade_media().unwrap();
        assert!(crate::test_hooks::storage_failure("delete_migration_before_commit", || root.upgrade_deletions()).is_err());
        let c = super::super::open(root.database(), false).unwrap();
        assert_eq!(c.pragma_query_value::<i64,_>(None, "user_version", |r| r.get(0)).unwrap(), 2);
        assert!(!c.prepare("SELECT 1 FROM sqlite_schema WHERE name='account_deletions'").unwrap().exists([]).unwrap());
        drop(c);
        root.check_initialized().unwrap();
        root.upgrade_deletions().unwrap();
        root.upgrade_deletions().unwrap();
        let c = super::super::open(root.database(), false).unwrap();
        assert_eq!(c.pragma_query_value::<i64,_>(None, "user_version", |r| r.get(0)).unwrap(), 3);
        assert!(pending(&c).unwrap().is_empty());
        root.check_initialized().unwrap();
    }
    #[test]
    fn complete_journal_validation_rejects_noncanonical_bindings_and_live_overlap() {
        let temp = tempfile::tempdir().unwrap();
        let mut root = DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
        root.init().unwrap(); root.upgrade_media().unwrap(); root.upgrade_deletions().unwrap();
        let c = super::super::open(root.database(), false).unwrap();
        let valid = crypto::random_token().unwrap();
        insert(&c, &Pending { id: valid.clone(), binding: Some((0, u64::MAX)) }).unwrap();
        assert_eq!(pending(&c).unwrap()[0].binding, Some((0, u64::MAX)));
        for bad in ["01", "-1", "18446744073709551616", " 1", "1.0"] {
            c.execute("UPDATE account_deletions SET device=?1 WHERE id=?2", params![bad,valid]).unwrap();
            assert!(root.check_initialized().is_err());
        }
        c.execute("UPDATE account_deletions SET device='0' WHERE id=?1", [&valid]).unwrap();
        c.execute("INSERT INTO account_deletions VALUES('not-an-id',NULL,NULL)", []).unwrap();
        assert!(pending(&c).is_err());
        c.execute("DELETE FROM account_deletions WHERE id='not-an-id'", []).unwrap();
        c.execute("DROP TRIGGER deletion_id_reserved", []).unwrap();
        c.execute("INSERT INTO accounts VALUES(?1,'overlap.member','member','provisioning',1,'synthetic',1,1,0)", [&valid]).unwrap();
        assert!(pending(&c).is_err());
    }
}
