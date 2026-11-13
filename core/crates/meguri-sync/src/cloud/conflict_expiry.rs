use super::{
    publication_store::WORK_TABLES,
    recovery::{NativeIdentity, RecoveryDirectory},
    store::{binding_id, decode, encode},
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const STAGING: &str = "meguri-cloud-staging";
// Additional actual tables not removed by publication_store::remove_work.
// Policy and incomplete rows have ON DELETE CASCADE, so FK refusal alone
// cannot protect them. Keep ended-exchange evidence with the policy evidence;
// publication cleanup does not acquire authority to clear it.
const EXTRA_WORK_TABLES: &[&str] = &[
    "cloud_apply_intents",
    "cloud_asset_first_reads",
    "cloud_policy_evidence",
    "cloud_policy_exchanges",
    "cloud_incomplete_bindings",
    "cloud_incomplete_legacy",
    "cloud_incomplete_ends",
    "cloud_incomplete_outgoing",
    "cloud_incomplete_incoming",
    "cloud_incomplete_results",
    "cloud_incomplete_flights",
];
const UNLINKS: &str = "CREATE TABLE IF NOT EXISTS cloud_conflict_unlinks(
 name TEXT PRIMARY KEY, evidence TEXT NOT NULL CHECK(json_valid(evidence)));";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Unlink {
    directory: NativeIdentity,
    file: NativeIdentity,
}

fn table_exists(c: &Connection, name: &str) -> Result<bool, Error> {
    let kind: Option<String> = c
        .query_row("SELECT type FROM sqlite_master WHERE name=?1", [name], |r| r.get(0))
        .optional()?;
    match kind.as_deref() {
        Some("table") => Ok(true),
        None => Ok(false),
        _ => Err(Error::InvalidData),
    }
}

fn asset_name(name: &str) -> bool {
    name.starts_with("asset-")
        && name.len() > "asset-".len()
        && matches!(
            Path::new(name).components().next(),
            Some(std::path::Component::Normal(_))
        )
        && Path::new(name).components().count() == 1
}

fn stage(root: &Path) -> Result<Option<RecoveryDirectory>, Error> {
    let path = root.join(STAGING);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(m) if !m.is_dir() => Err(Error::RecoveryRefused),
        Ok(_) => RecoveryDirectory::open(&path).map(Some),
    }
}

// A copy's opaque path is not deletion authority. Recognize only the actual
// media staging producer's direct asset-* namespace, with no links/hard links.
fn capture(root: &Path, path: &Path) -> Result<Option<(String, Unlink)>, Error> {
    if path.parent() != Some(root.join(STAGING).as_path()) {
        return Ok(None);
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()).filter(|n| asset_name(n)) else {
        return Ok(None);
    };
    let Some(directory) = stage(root)? else { return Ok(None) };
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        _ => {}
    }
    // Refuse a changed inode or a non-regular entry. Never fall back to unlink
    // using the stored path when validation fails.
    let Some(file) = directory.entry_identity(name)? else {
        return Ok(None);
    };
    Ok(Some((
        name.into(),
        Unlink {
            directory: directory.native_identity(),
            file,
        },
    )))
}

fn contents<T: serde::de::DeserializeOwned>(c: &Connection, table: &str, column: &str) -> Result<Vec<T>, Error> {
    if !table_exists(c, table)? {
        return Ok(Vec::new());
    }
    let mut q = c.prepare(&format!("SELECT {column} FROM {table}"))?;
    q.query_map([], |r| r.get::<_, String>(0))?
        .map(|r| decode(&r?))
        .collect()
}

/// Read references across every build/owner, without selecting any of them.
/// Unknown/unreadable storage fails closed instead of authorizing a sweep.
fn references(c: &Connection, root: &Path) -> Result<BTreeSet<PathBuf>, Error> {
    let mut paths = BTreeSet::new();
    for copy in contents::<ConflictCopy>(c, "conflicts", "content")? {
        paths.extend(copy.staged_path.map(PathBuf::from));
    }
    for entry in contents::<InboxEntry>(c, "inbox", "entry")? {
        if let InboxValue::Record { staged_asset, .. } = entry.value {
            paths.extend(staged_asset.map(PathBuf::from));
        }
    }
    for ledger in contents::<media::Ledger>(c, "cloud_media_state", "content")? {
        paths.extend(ledger.blobs.into_values().filter_map(|b| b.staged));
    }
    for intent in contents::<incoming::ApplyIntent>(c, "cloud_apply_intents", "content")? {
        paths.extend(intent.media.and_then(|m| m.staged));
    }
    for op in contents::<adoption::AdoptOp>(c, "cloud_adoption_ops", "content")? {
        paths.insert(root.join(op.staging_name));
    }
    // PublishOp has no staging field. Its binding references are work, below.
    Ok(paths)
}

fn retained(paths: &BTreeSet<PathBuf>, path: &Path) -> bool {
    // Recovery ops may retain a directory rather than each individual asset.
    paths.iter().any(|p| path == p || path.starts_with(p))
}

fn has_work(tx: &Transaction<'_>, owner: BindingId) -> Result<bool, Error> {
    for table in WORK_TABLES.iter().chain(EXTRA_WORK_TABLES).copied() {
        if table_exists(tx, table)?
            && tx.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE binding=?1)"),
                [owner.0.as_slice()],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Ok(true);
        }
    }
    // Re-key rows are owned through their apply intent, not a binding column.
    if table_exists(tx, "cloud_rekey_intents")? && tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cloud_rekey_intents r JOIN cloud_apply_intents i ON i.id=r.id WHERE i.binding=?1)",
        [owner.0.as_slice()], |r| r.get::<_, bool>(0),
    )? { return Ok(true); }
    // A recovery decision must not lose its before/after owner, including a
    // foreign build's op. These reads grant no authority to settle that op.
    for op in contents::<publication::PublishOp>(tx, "cloud_publication_ops", "content")? {
        if op.binding_after == Some(owner) || op.binding_before.is_some_and(|b| b.id == owner) {
            return Ok(true);
        }
    }
    for op in contents::<adoption::AdoptOp>(tx, "cloud_adoption_ops", "content")? {
        if op.binding_after == owner || op.binding_before.is_some_and(|b| b.id == owner) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn queue(tx: &Transaction<'_>, name: &str, evidence: &Unlink) -> Result<(), Error> {
    let encoded = encode(evidence)?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT evidence FROM cloud_conflict_unlinks WHERE name=?1",
            [name],
            |r| r.get(0),
        )
        .optional()?;
    if previous.as_ref().is_some_and(|p| p != &encoded) {
        return Err(Error::RecoveryRefused);
    }
    tx.execute(
        "INSERT INTO cloud_conflict_unlinks(name,evidence) VALUES(?1,?2) ON CONFLICT(name) DO NOTHING",
        params![name, encoded],
    )?;
    Ok(())
}

pub(super) fn expire_in_transaction(tx: &Transaction<'_>, root: &Path, now_ms: i64) -> Result<usize, Error> {
    tx.execute_batch(UNLINKS)?;
    let rows: Vec<(Vec<u8>, String)> = {
        let mut q = tx.prepare("SELECT binding,content FROM conflicts ORDER BY id")?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    let mut owners = BTreeSet::new();
    let mut staged = Vec::new();
    let mut count = 0;
    for (owner, content) in rows {
        let copy: ConflictCopy = decode(&content)?;
        if !copy
            .resolved_at_ms
            .and_then(|resolved| now_ms.checked_sub(resolved))
            .is_some_and(|age| age >= RETENTION_MS)
        {
            continue;
        }
        if copy.kind == ConflictKind::MediaName
            && let Some(path) = copy.staged_path.as_deref()
            && let Some(owned) = capture(root, Path::new(path))?
        {
            staged.push(owned);
        }
        tx.execute("DELETE FROM conflicts WHERE id=?1", [copy.id.0.as_slice()])?;
        owners.insert(owner);
        count += 1;
    }
    let live = references(tx, root)?;
    for (name, evidence) in staged {
        if !retained(&live, &root.join(STAGING).join(&name)) {
            queue(tx, &name, &evidence)?;
        }
    }
    for owner in owners {
        let owner = binding_id(owner)?;
        let remaining: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM conflicts WHERE binding=?1)",
            [owner.0.as_slice()],
            |r| r.get(0),
        )?;
        if remaining || has_work(tx, owner)? {
            continue;
        }
        tx.execute("DELETE FROM bindings WHERE id=?1 AND active=0", [owner.0.as_slice()])?;
        tx.execute("DELETE FROM salvage_bindings WHERE id=?1", [owner.0.as_slice()])?;
    }
    Ok(count)
}

impl TrackingStore {
    /// P10.3 item 4's single transaction, across ALL owners/builds. Caller
    /// excludes other tracking and staging writers through finish/recovery.
    /// The commit records owned unlink evidence; this method removes no file.
    /// Supplied resolved timestamps are storage facts, not UI resolve proof.
    pub fn expire_resolved_conflicts(&mut self, now_ms: i64) -> Result<usize, Error> {
        let root = &self.directory;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count = expire_in_transaction(&tx, root, now_ms)?;
        tx.commit()?;
        Ok(count)
    }

    /// Replay only committed staging unlinks, also after a failed finish or a
    /// process death. Failed validation/unlink/fsync keeps durable evidence.
    /// Live references and replaced files are never removed.
    pub fn finish_conflict_expiry(&mut self) -> Result<(), Error> {
        if !table_exists(&self.connection, "cloud_conflict_unlinks")? {
            return Ok(());
        }
        let rows: Vec<(String, String)> = {
            let mut q = self
                .connection
                .prepare("SELECT name,evidence FROM cloud_conflict_unlinks ORDER BY name")?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?
        };
        if rows.is_empty() {
            return Ok(());
        }
        let live = references(&self.connection, &self.directory)?;
        let directory = stage(&self.directory)?.ok_or(Error::RecoveryRefused)?;
        for (name, content) in rows {
            if !asset_name(&name) {
                return Err(Error::RecoveryRefused);
            }
            let evidence: Unlink = decode(&content)?;
            if evidence.directory != directory.native_identity() {
                return Err(Error::RecoveryRefused);
            }
            if retained(&live, &self.directory.join(STAGING).join(&name)) {
                continue;
            }
            directory.remove_owned(&name, evidence.file)?;
            self.connection.execute(
                "DELETE FROM cloud_conflict_unlinks WHERE name=?1 AND evidence=?2",
                params![name, content],
            )?;
        }
        Ok(())
    }

    /// V2.4 step 6 for the existing direct media asset namespace. Payload
    /// subdirectories retain their existing lease-aware sweeps. Never walk
    /// live media, recovery sources, another directory, or arbitrary paths.
    pub(super) fn sweep_conflict_staging(&mut self) -> Result<(), Error> {
        self.finish_conflict_expiry()?;
        let Some(_pinned) = stage(&self.directory)? else {
            return Ok(());
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(UNLINKS)?;
        let live = references(&tx, &self.directory)?;
        for entry in fs::read_dir(self.directory.join(STAGING))? {
            let entry = entry?;
            let name = entry.file_name();
            if !name.to_str().is_some_and(asset_name) || retained(&live, &entry.path()) {
                continue;
            }
            if let Some((name, evidence)) = capture(&self.directory, &entry.path())? {
                queue(&tx, &name, &evidence)?;
            }
        }
        tx.commit()?;
        self.finish_conflict_expiry()
    }
}

pub(super) fn validate(c: &Connection) -> Result<(), Error> {
    if table_exists(c, "cloud_conflict_unlinks")? {
        let mut q = c.prepare("SELECT name,evidence FROM cloud_conflict_unlinks")?;
        let mut rows = q.query([])?;
        while let Some(row) = rows.next()? {
            if !asset_name(&row.get::<_, String>(0)?) {
                return Err(Error::InvalidData);
            }
            let _: Unlink = decode(&row.get::<_, String>(1)?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "conflict_expiry_tests.rs"]
mod tests;
