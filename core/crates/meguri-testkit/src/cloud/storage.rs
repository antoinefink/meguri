use super::types::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

// SQLite INTEGER parameters must fit its signed storage domain.
pub(super) fn write_u64(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::Exhausted)
}
pub(super) fn write_usize(value: usize) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::Exhausted)
}

pub(super) fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    checked_u64(row.get(index)?, index)
}
pub(super) fn read_usize(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value: i64 = row.get(index)?;
    usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
pub(super) fn read_optional_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| checked_u64(value, index))
        .transpose()
}
fn checked_u64(value: i64, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, StorageError> {
    Ok(serde_json::to_vec(value)?)
}
pub(super) fn decode<T: DeserializeOwned>(value: Vec<u8>) -> Result<T, StorageError> {
    Ok(serde_json::from_slice(&value)?)
}
pub(super) fn initialize(c: &Connection, config: &Config) -> Result<(), StorageError> {
    let tables: i64 = c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    if tables != 0 {
        let identity: (i64, i64) = c.query_row("SELECT format, revision FROM service_meta WHERE id=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        if identity != (117, 1) {
            return Err(StorageError::Database);
        }
        return Ok(());
    }
    c.execute_batch("
        CREATE TABLE service_meta(id INTEGER PRIMARY KEY CHECK(id=1), format INTEGER NOT NULL, revision INTEGER NOT NULL,
            config BLOB NOT NULL, request INTEGER NOT NULL, sequence INTEGER NOT NULL, version INTEGER NOT NULL);
        CREATE TABLE databases(scope TEXT PRIMARY KEY, identity BLOB NOT NULL, config BLOB NOT NULL, quota BLOB, used INTEGER NOT NULL);
        CREATE TABLE zones(scope TEXT NOT NULL, zone TEXT NOT NULL, generation INTEGER NOT NULL, removed BLOB, used INTEGER NOT NULL,
            PRIMARY KEY(scope,zone));
        CREATE TABLE payloads(hash BLOB PRIMARY KEY, body BLOB NOT NULL);
        CREATE TABLE versions(id INTEGER PRIMARY KEY, scope TEXT NOT NULL, zone TEXT NOT NULL, generation INTEGER NOT NULL,
            name TEXT NOT NULL, record_type TEXT NOT NULL, system BLOB NOT NULL UNIQUE, size INTEGER NOT NULL,
            encrypted BLOB NOT NULL REFERENCES payloads(hash), asset_key TEXT, asset BLOB REFERENCES payloads(hash));
        CREATE TABLE current_records(scope TEXT NOT NULL, zone TEXT NOT NULL, name TEXT NOT NULL,
            version INTEGER NOT NULL REFERENCES versions(id), PRIMARY KEY(scope,zone,name));
        CREATE TABLE record_changes(sequence INTEGER PRIMARY KEY, scope TEXT NOT NULL, zone TEXT NOT NULL, generation INTEGER NOT NULL,
            name TEXT NOT NULL, record_type TEXT NOT NULL, version INTEGER REFERENCES versions(id));
        CREATE INDEX record_changes_scope ON record_changes(scope,zone,sequence,name);
        CREATE TABLE zone_changes(sequence INTEGER PRIMARY KEY, scope TEXT NOT NULL, zone TEXT NOT NULL, removed BLOB);
        CREATE INDEX zone_changes_scope ON zone_changes(scope,sequence,zone);
        CREATE TABLE requests(id INTEGER PRIMARY KEY, log BLOB NOT NULL, outcome BLOB NOT NULL);
        CREATE TABLE snapshots(id INTEGER PRIMARY KEY, scope TEXT NOT NULL, zone TEXT, generation INTEGER NOT NULL,
            watermark INTEGER NOT NULL, count INTEGER NOT NULL);
        CREATE TABLE members(snapshot INTEGER NOT NULL REFERENCES snapshots(id), name TEXT NOT NULL,
            version INTEGER REFERENCES versions(id), record_type TEXT, removed BLOB, PRIMARY KEY(snapshot,name));
    ")?;
    c.execute("INSERT INTO service_meta VALUES(1,117,1,?1,0,0,0)", [encode(config)?])?;
    Ok(())
}
pub(super) fn counter(c: &Connection, name: &str) -> Result<u64, StorageError> {
    let sql = match name {
        "request" => "UPDATE service_meta SET request=request+1 WHERE request<9223372036854775807 RETURNING request",
        "sequence" => {
            "UPDATE service_meta SET sequence=sequence+1 WHERE sequence<9223372036854775807 RETURNING sequence"
        }
        "version" => "UPDATE service_meta SET version=version+1 WHERE version<9223372036854775807 RETURNING version",
        _ => return Err(StorageError::Database),
    };
    c.query_row(sql, [], |r| read_u64(r, 0))
        .optional()?
        .ok_or(StorageError::Exhausted)
}
pub(super) fn scope_key(s: &Scope) -> String {
    format!(
        "{}:{}{}:{}{}:{}",
        s.container.len(),
        s.container,
        s.environment.len(),
        s.environment,
        s.account.len(),
        s.account
    )
}
pub(super) fn ensure_scope(c: &Connection, s: &Scope) -> Result<String, StorageError> {
    let key = scope_key(s);
    c.execute(
        "INSERT OR IGNORE INTO databases SELECT ?1,?2,config,NULL,0 FROM service_meta WHERE id=1",
        params![key, encode(s)?],
    )?;
    Ok(key)
}
pub(super) fn config(c: &Connection, key: &str) -> Result<Config, StorageError> {
    decode(c.query_row(
        "SELECT config FROM databases WHERE scope=?1 UNION ALL SELECT config FROM service_meta WHERE id=1 LIMIT 1",
        [key],
        |r| r.get(0),
    )?)
}
pub(super) fn payload(c: &Connection, bytes: &[u8]) -> Result<Vec<u8>, StorageError> {
    let hash = Sha256::digest(bytes).to_vec();
    c.execute("INSERT OR IGNORE INTO payloads VALUES(?1,?2)", params![hash, bytes])?;
    Ok(hash)
}
pub(super) fn save_version(
    c: &Connection,
    key: &str,
    zone: &str,
    generation: u64,
    id: u64,
    record: &Record,
    size: u64,
) -> Result<(), StorageError> {
    let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
        return Err(StorageError::Encoding);
    };
    let encrypted = payload(c, e)?;
    let asset = record.fields.iter().find_map(|(key, value)| match value {
        Field::Asset(a) => Some((key, a)),
        _ => None,
    });
    let asset_hash = asset.map(|(_, a)| payload(c, a.bytes())).transpose()?;
    c.execute(
        "INSERT INTO versions VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            write_u64(id)?,
            key,
            zone,
            write_u64(generation)?,
            record.name,
            record.record_type,
            record.system_fields.bytes(),
            write_u64(size)?,
            encrypted,
            asset.map(|(k, _)| k),
            asset_hash
        ],
    )?;
    Ok(())
}
type RecordColumns = (String, String, Vec<u8>, Vec<u8>, Option<String>, Option<Vec<u8>>);

pub(super) fn record(c: &Connection, id: u64, selected: &RequestedFields) -> Result<Record, StorageError> {
    let (name, record_type, system, encrypted, asset_key, asset): RecordColumns = c.query_row(
        "SELECT name,record_type,system,encrypted,asset_key,asset FROM versions WHERE id=?1",
        [write_u64(id)?],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    let want = |key: &str| match selected {
        RequestedFields::All => true,
        RequestedFields::Named(names) => names.iter().any(|n| n == key),
    };
    let mut fields = BTreeMap::new();
    if want("e") {
        let bytes = c.query_row("SELECT body FROM payloads WHERE hash=?1", [encrypted], |r| r.get(0))?;
        fields.insert("e".into(), Field::EncryptedBytes(bytes));
    }
    if let (Some(key), Some(hash)) = (asset_key, asset)
        && want(&key)
    {
        let bytes = c.query_row("SELECT body FROM payloads WHERE hash=?1", [hash], |r| r.get(0))?;
        fields.insert(key, Field::Asset(Asset::synthetic(bytes)));
    }
    Ok(Record {
        name,
        record_type,
        fields,
        system_fields: SystemFields(system),
    })
}
pub(super) fn reference(c: &Connection, record: &Record) -> Result<RecordReference, StorageError> {
    let version = c.query_row(
        "SELECT id FROM versions WHERE system=?1",
        [record.system_fields.bytes()],
        |r| read_u64(r, 0),
    )?;
    Ok(RecordReference {
        version,
        fields: record.fields.keys().cloned().collect(),
    })
}
pub(super) fn error_reference(c: &Connection, e: &ServiceError) -> Result<ErrorReference, StorageError> {
    Ok(ErrorReference {
        code: e.code,
        encrypted_data_reset: e.encrypted_data_reset,
        server_record: e.server_record.as_deref().map(|r| reference(c, r)).transpose()?,
    })
}
pub(super) fn outcome(c: &Connection, response: &Response) -> Result<OutcomeReference, StorageError> {
    let result = match &response.result {
        Err(e) => Err(error_reference(c, e)?),
        Ok(reply) => Ok(match reply {
            Reply::Done => ReplyReference::Done,
            Reply::Record(r) => ReplyReference::Record(reference(c, r)?),
            Reply::Modified(items) => ReplyReference::Modified(
                items
                    .iter()
                    .map(|item| {
                        Ok(MutationReference {
                            name: item.name.clone(),
                            result: match &item.result {
                                Ok(MutationSuccess::Deleted) => Ok(None),
                                Ok(MutationSuccess::Saved(r)) => Ok(Some(reference(c, r)?)),
                                Err(e) => Err(error_reference(c, e)?),
                            },
                        })
                    })
                    .collect::<Result<_, StorageError>>()?,
            ),
            Reply::ZoneChanges(p) => ReplyReference::ZoneChanges(Page {
                token: p.token.clone(),
                more: p.more,
                changes: p
                    .changes
                    .iter()
                    .map(|change| {
                        Ok(match change {
                            RecordChange::Saved(r) => ChangeReference::Saved(reference(c, r)?),
                            RecordChange::Deleted { name, record_type } => ChangeReference::Deleted {
                                name: name.clone(),
                                record_type: record_type.clone(),
                            },
                        })
                    })
                    .collect::<Result<_, StorageError>>()?,
            }),
            Reply::DatabaseChanges(p) => ReplyReference::DatabaseChanges(p.clone()),
        }),
    };
    Ok(OutcomeReference {
        request_id: response.request_id,
        result,
    })
}
